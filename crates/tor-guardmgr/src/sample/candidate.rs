//! This module defines and implements traits used to create a guard sample from
//! either bridges or relays.

use std::time::SystemTime;

use tor_linkspec::{ByRelayIds, HasRelayIds, OwnedChanTarget};
use tor_netdir::{NetDir, Relay, RelayWeight};

use crate::{GuardFilter, GuardParams};

/// A "Universe" is a source from which guard candidates are drawn, and from
/// which guards are updated.
pub(crate) trait Universe {
    /// Check whether this universe contains a candidate with the given
    /// identities.
    ///
    /// Return `Some(true)` if it definitely does; `Some(false)` if it
    /// definitely does not, and `None` if we cannot tell without downloading
    /// more information.
    fn contains<T: HasRelayIds>(&self, id: &T) -> Option<bool>;

    /// Return full information about a member of this universe, by its identity.
    fn status<T: HasRelayIds>(&self, id: &T) -> CandidateStatus;

    /// Return the time at which this Universe last changed.  This can be
    /// approximate.
    fn timestamp(&self) -> SystemTime;

    /// Return information about how much of this universe has been added to
    /// `sample`, and how much we're willing to add according to `params`.
    fn weight_threshold<T>(&self, sample: &ByRelayIds<T>, params: &GuardParams) -> WeightThreshold
    where
        T: HasRelayIds;

    /// Return up to `n` of new candidate guards from this Universe.
    ///
    /// Only return elements that have no conflicts with identities in
    /// `pre_existing`, and which obey `filter`.
    fn sample<T>(
        &self,
        pre_existing: &ByRelayIds<T>,
        filter: &GuardFilter,
        n: usize,
    ) -> Vec<(OwnedChanTarget, RelayWeight)>
    where
        T: HasRelayIds;
}

/// Information about a single guard candidate, as returned by
/// [`Universe::status`].
#[derive(Clone, Debug)]
pub(crate) enum CandidateStatus {
    /// The candidate is definitely present in some form.
    Present {
        /// True if the candidate is not currently disabled for use as a guard.
        listed_as_guard: bool,
        /// True if the candidate can be used as a directory cache.
        is_dir_cache: bool,
        /// Information about connecting to the candidate and using it to build
        /// a channel.
        owned_target: OwnedChanTarget,
    },
    /// The candidate is definitely not in the [`Universe`].
    Absent,
    /// We would need to download more directory information to be sure whether
    /// this candidate is in the [`Universe`].
    Uncertain,
}

/// Information about how much of the universe we are using in a guard sample,
/// and how much we are allowed to use.
///
/// We use this to avoid adding the whole network to our guard sample.
#[derive(Debug, Clone)]
pub(crate) struct WeightThreshold {
    /// The amount of the universe that we are using, in [`RelayWeight`].
    pub(crate) current_weight: RelayWeight,
    /// The greatest amount that we are willing to use, in [`RelayWeight`].
    ///
    /// We can violate this maximum if it's necessary in order to meet our
    /// minimum number of guards; otherwise, were're willing to add a _single_
    /// guard that exceeds this threshold, but no more.
    pub(crate) maximum_weight: RelayWeight,
}

impl Universe for NetDir {
    fn timestamp(&self) -> SystemTime {
        NetDir::lifetime(self).valid_after()
    }

    fn contains<T: HasRelayIds>(&self, id: &T) -> Option<bool> {
        NetDir::ids_listed(self, id)
    }

    fn status<T: HasRelayIds>(&self, id: &T) -> CandidateStatus {
        match NetDir::by_ids(self, id) {
            Some(relay) => CandidateStatus::Present {
                listed_as_guard: relay.is_flagged_guard(),
                is_dir_cache: relay.is_dir_cache(),
                owned_target: OwnedChanTarget::from_chan_target(&relay),
            },
            None => match NetDir::ids_listed(self, id) {
                Some(true) => panic!("ids_listed said true, but by_ids said none!"),
                Some(false) => CandidateStatus::Absent,
                None => CandidateStatus::Uncertain,
            },
        }
    }

    fn weight_threshold<T>(&self, sample: &ByRelayIds<T>, params: &GuardParams) -> WeightThreshold
    where
        T: HasRelayIds,
    {
        // When adding from a netdir, we impose total limit on the fraction of
        // the universe we're willing to add.
        let maximum_weight = {
            let total_weight = self.total_weight(tor_netdir::WeightRole::Guard, |r| {
                r.is_flagged_guard() && r.is_dir_cache()
            });
            total_weight
                .ratio(params.max_sample_bw_fraction)
                .unwrap_or(total_weight)
        };

        let current_weight: tor_netdir::RelayWeight = sample
            .values()
            .filter_map(|guard| {
                self.weight_by_rsa_id(guard.rsa_identity()?, tor_netdir::WeightRole::Guard)
            })
            .sum();

        WeightThreshold {
            current_weight,
            maximum_weight,
        }
    }

    fn sample<T>(
        &self,
        pre_existing: &ByRelayIds<T>,
        filter: &GuardFilter,
        n: usize,
    ) -> Vec<(OwnedChanTarget, RelayWeight)>
    where
        T: HasRelayIds,
    {
        /// Return the weight for this relay, if we can find it.
        ///
        /// (We should always be able to find it as netdirs are constructed
        /// today.)
        fn weight(dir: &NetDir, relay: &Relay<'_>) -> Option<RelayWeight> {
            dir.weight_by_rsa_id(relay.rsa_identity()?, tor_netdir::WeightRole::Guard)
        }

        self.pick_n_relays(
            &mut rand::thread_rng(),
            n,
            tor_netdir::WeightRole::Guard,
            |relay| {
                filter.permits(relay)
                    && relay.is_flagged_guard()
                    && relay.is_dir_cache()
                    && pre_existing.all_overlapping(relay).is_empty()
            },
        )
        .iter()
        .map(|relay| {
            (
                OwnedChanTarget::from_chan_target(relay),
                weight(self, relay).unwrap_or_else(|| RelayWeight::from(0)),
            )
        })
        .collect()
    }
}
