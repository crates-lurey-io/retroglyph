//! [`Consumed`]: whether [`Interaction::handle_event`](crate::Interaction::handle_event) claimed
//! an event.

/// Whether an [`Interaction`](crate::Interaction) claimed an event.
///
/// Reported by [`handle_event`](crate::Interaction::handle_event), so a caller with more than one
/// interaction context (a menu bar above a screen stack, a dropdown above a form) knows whether to
/// keep routing the same event further down. A coarse substitute, like "is the menu currently
/// open", answers a different and less useful question: it stays `true` for events the menu
/// doesn't care about (`Resize`, `Paste`, a key it doesn't bind), and it depends on the caller
/// keeping that flag in sync with state that can change out from under it (closing the menu
/// without also clearing the flag). `Consumed` instead reports, per event, whether *this* call
/// actually used it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Consumed {
    /// Nothing here claimed the event: keep routing it to whatever's behind this interaction.
    No,
    /// This interaction claimed the event: stop routing it further.
    Yes,
}

impl Consumed {
    /// `true` for [`Consumed::Yes`].
    #[must_use]
    pub const fn is_yes(self) -> bool {
        matches!(self, Self::Yes)
    }

    /// `true` for [`Consumed::No`].
    #[must_use]
    pub const fn is_no(self) -> bool {
        matches!(self, Self::No)
    }
}

impl From<bool> for Consumed {
    /// `true` becomes [`Consumed::Yes`], `false` becomes [`Consumed::No`].
    fn from(consumed: bool) -> Self {
        if consumed { Self::Yes } else { Self::No }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_yes_and_is_no_agree_with_the_variant() {
        assert!(Consumed::Yes.is_yes());
        assert!(!Consumed::Yes.is_no());
        assert!(Consumed::No.is_no());
        assert!(!Consumed::No.is_yes());
    }

    #[test]
    fn from_bool_maps_true_and_false() {
        assert_eq!(Consumed::from(true), Consumed::Yes);
        assert_eq!(Consumed::from(false), Consumed::No);
    }
}
