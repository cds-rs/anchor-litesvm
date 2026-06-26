#![no_std]
//! Declare a Pinocchio program's instruction and error vocabulary *once*, at
//! the point the program already reconciles it, and get both the on-chain code
//! and the `(code, name)` tables the host-side tests need from one source.
//!
//! ## The problem this solves
//!
//! A Pinocchio program has no IDL. The runtime logs an instruction only by its
//! discriminator byte and a failure only by its raw `custom program error:
//! 0x<code>`; nothing spells out `Make` or `InvalidAmount`. So a test driving
//! such a program through [`litesvm-utils`](https://docs.rs/litesvm-utils)
//! renders frames by the bare program alias and can't match an error by name.
//! `litesvm-utils` closes that with two registries (`InstructionNames` /
//! `ErrorNames`): the test registers `code -> name` and the renderers and
//! `send_err_named` speak names again.
//!
//! The open question is where the `code -> name` table comes from. Hand-written,
//! it drifts from the discriminators the program actually matches on. These
//! macros remove the drift by making the *declaration* the single source: the
//! variant identifier is the name (`stringify!`d), so you cannot add an
//! instruction or error without it appearing, correctly named, in the table,
//! and cannot rename a variant without the name following.
//!
//! ## What's emitted, and why it's BPF-safe
//!
//! The macros emit program code (the `enum`, its discriminator consts, the
//! `From<_> for ProgramError` conversion) on *all* targets, plus the
//! `*_names()` accessor gated behind `#[cfg(not(target_os = "solana"))]` so it's
//! absent from the on-chain binary. This crate carries no runtime dependencies:
//! the emitted `::core::*` and `::pinocchio::*` paths resolve in the program
//! that invokes the macro, not here, so the crate itself builds for the BPF
//! target and can sit in a program's normal `[dependencies]`.
//!
//! ## Usage
//!
//! ```text
//! use litesvm_pinocchio::{define_instruction_set, define_error_set};
//!
//! define_instruction_set! {
//!     pub enum EscrowInstruction {
//!         0 => Make(MakeArgs),
//!         1 => Take,
//!         2 => Cancel,
//!     }
//! }
//!
//! define_error_set! {
//!     #[repr(u32)]
//!     pub enum EscrowError {
//!         0 => InvalidInstruction,
//!         7 => InvalidAmount,
//!     }
//! }
//! ```
//!
//! Then, in the test:
//!
//! ```text
//! ctx.register_program_instructions(PROGRAM_ID, EscrowInstruction::instruction_names());
//! ctx.register_program_errors(PROGRAM_ID, EscrowError::error_names());
//! ```
//!
//! The same sockets exist on every `testsvm::TestSVM` backend (litesvm,
//! mollusk, RPC), so the generated tables plug into any engine:
//!
//! ```text
//! backend.register_program_instructions(&PROGRAM_ID, EscrowInstruction::instruction_names());
//! backend.register_program_errors(&PROGRAM_ID, EscrowError::error_names());
//! ```
//!
//! ## `define_instruction_set!` vs `#[derive(Discriminator)]`
//!
//! `define_instruction_set!` is a function-like macro: it *replaces* the enum,
//! so a source parser (Shank, or our own IDL extractor) sees only a token blob
//! and cannot recover the instruction set. When you want the program to also
//! yield an IDL, reach for [`Discriminator`] instead: it derives the same
//! discriminator consts and name table, but as an *additive* derive the plain
//! enum stays in the source for the host-side extractor to read. Carry the
//! per-instruction account list in `#[account(..)]` helper attributes on the
//! variants (inert: zero bytes on-chain, and `no_std`-clean).

pub use litesvm_pinocchio_derive::Discriminator;

/// Declare a program's instruction set: the `enum` the dispatcher matches on,
/// the discriminator consts `unpack` matches the wire byte against, and a
/// host-only `instruction_names()` table for the test registry.
///
/// `0 => Make(MakeArgs)` is read as three independent facts: the discriminator
/// is `0`, the name is `"Make"` (the variant identifier, stringified), and
/// `(MakeArgs)` is payload carried into the enum untouched (a unit variant like
/// `1 => Take` simply has none). The payload never participates in naming.
///
/// Expands to (all targets):
///
/// - the `enum`, with payloads preserved;
/// - a sibling `discriminators` module of `pub const`s named after the
///   variants, for matching the wire byte (`discriminators::Make`, etc.);
///
/// and (host builds only, `#[cfg(not(target_os = "solana"))]`):
///
/// - `<Enum>::instruction_names() -> &'static [(u8, &'static str)]`, the table
///   to hand to the test harness's instruction registry.
#[macro_export]
macro_rules! define_instruction_set {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident {
            $( $disc:literal => $variant:ident $(( $($payload:tt)* ))? ),+ $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        $vis enum $name {
            $( $variant $(( $($payload)* ))? ),+
        }

        /// One-byte discriminators, each named after its variant, for matching
        /// the leading instruction-data byte. The lint allow is deliberate: the
        /// const mirrors the variant identifier (`Make`), not a SCREAMING_CASE
        /// convention, so the call site reads `discriminators::Make`.
        #[allow(non_upper_case_globals)]
        $vis mod discriminators {
            $( pub const $variant: u8 = $disc; )+
        }

        // Host-only: the renderer-facing name table. Excluded from the SBF
        // build (`target_os = "solana"`), where it would be dead weight.
        #[cfg(not(target_os = "solana"))]
        impl $name {
            /// `(discriminator, name)` for every instruction, the names taken
            /// from the variant identifiers. Pass to the test harness's
            /// instruction registry (e.g.
            /// `ctx.register_program_instructions(ID, EscrowInstruction::instruction_names())`).
            $vis fn instruction_names() -> &'static [(u8, &'static str)] {
                &[ $( ($disc, ::core::stringify!($variant)) ),+ ]
            }
        }
    };
}

/// The failure-path twin of [`define_instruction_set!`]: declare a program's
/// custom-error set once and get the `enum`, its
/// `From<Self> for pinocchio::error::ProgramError` conversion, and a host-only
/// `error_names()` table the test registry consumes so a `ProgramError::Custom(n)`
/// renders and matches by name (`InvalidAmount`) instead of `0x<n>`.
///
/// `7 => InvalidAmount` is read as: the `Custom` code is `7`, the name is
/// `"InvalidAmount"` (the variant identifier).
///
/// Expands to (all targets) the enum with explicit discriminants and the
/// `From<Self> for ::pinocchio::error::ProgramError` impl that maps the variant
/// to `Custom(code)`; plus (host builds only) `<Enum>::error_names() -> &'static
/// [(u32, &'static str)]`.
///
/// The macro does *not* dictate the enum's `repr`: write `#[repr(u8)]`,
/// `#[repr(u32)]`, or nothing, and it passes through. `ProgramError::Custom` is
/// always a `u32` on the wire, and `value as u32` widens from any unsigned repr
/// (and from the default repr when you give none), so the registry codes stay
/// `u32` regardless of how the enum is laid out in memory.
#[macro_export]
macro_rules! define_error_set {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident {
            $( $code:literal => $variant:ident ),+ $(,)?
        }
    ) => {
        // No `repr` forced here: the discriminants make `value as u32` read the
        // codes back whatever the layout, so the caller owns the `repr` choice.
        $(#[$enum_meta])*
        $vis enum $name {
            $( $variant = $code ),+
        }

        impl ::core::convert::From<$name> for ::pinocchio::error::ProgramError {
            fn from(value: $name) -> Self {
                ::pinocchio::error::ProgramError::Custom(value as u32)
            }
        }

        // Host-only: the renderer/assert-facing name table. Excluded from the
        // SBF build, where it would be dead weight.
        #[cfg(not(target_os = "solana"))]
        impl $name {
            /// `(code, name)` for every error, the names taken from the variant
            /// identifiers. Pass to the test harness's error registry (e.g.
            /// `ctx.register_program_errors(ID, EscrowError::error_names())`).
            $vis fn error_names() -> &'static [(u32, &'static str)] {
                &[ $( ($code, ::core::stringify!($variant)) ),+ ]
            }
        }
    };
}

#[cfg(test)]
mod tests {
    // Exercise the macros end to end: declare a set, then assert the generated
    // consts, the name tables, and (for errors) the ProgramError conversion.
    // `pinocchio` is a dev-dependency so the `From<_> for ProgramError` impl the
    // error macro emits actually compiles and runs here.
    use pinocchio::error::ProgramError;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct MakeArgs {
        pub seed: u64,
    }

    // `#[allow(dead_code)]` rides through the macro's meta passthrough: these
    // are fixtures, and not every variant is constructed in the assertions.
    crate::define_instruction_set! {
        #[allow(dead_code)]
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Ix {
            0 => Make(MakeArgs),
            1 => Take,
            2 => Cancel,
        }
    }

    crate::define_error_set! {
        #[allow(dead_code)]
        #[repr(u32)]
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Err {
            0 => InvalidInstruction,
            7 => InvalidAmount,
        }
    }

    // A second error set with a compact repr, to prove the `repr` passes through
    // and the codes still widen to u32 for `Custom`.
    crate::define_error_set! {
        #[allow(dead_code)]
        #[repr(u8)]
        pub enum SmallErr {
            3 => Tiny,
        }
    }

    #[test]
    fn instruction_consts_and_names_track_the_variants() {
        assert_eq!(discriminators::Make, 0);
        assert_eq!(discriminators::Take, 1);
        assert_eq!(discriminators::Cancel, 2);
        assert_eq!(
            Ix::instruction_names(),
            &[(0, "Make"), (1, "Take"), (2, "Cancel")]
        );
        // The payload rides into the enum but never touches the name.
        let _ = Ix::Make(MakeArgs { seed: 7 });
    }

    #[test]
    fn error_names_and_conversion_track_the_variants() {
        assert_eq!(
            Err::error_names(),
            &[(0, "InvalidInstruction"), (7, "InvalidAmount")]
        );
        assert_eq!(
            ProgramError::from(Err::InvalidAmount),
            ProgramError::Custom(7)
        );
    }

    #[test]
    fn repr_passes_through_and_codes_stay_u32() {
        // repr(u8) enum, but the registered code and the wire conversion are u32.
        assert_eq!(SmallErr::error_names(), &[(3u32, "Tiny")]);
        assert_eq!(ProgramError::from(SmallErr::Tiny), ProgramError::Custom(3));
    }
}
