# Failure Modes

Indexed by the literal error text. Search this page for the string you are
holding.

| error contains | cause | fix |
|---|---|---|
| `AlreadyProcessed` / `already been processed` | identical transaction, same blockhash, sent through raw litesvm | send through the helpers (fresh blockhash per send), or `ctx.svm.expire_blockhash()` before the raw resend |
| `EntrypointOutOfBounds` | the `.so` is an entrypoint-less stub (feature-gated `entrypoint!` compiled out) | rebuild with the program's SBF features, e.g. `cargo build-sbf -- --no-default-features --features sbf` |
| `Unknown program` (during a token CPI) | `token_program` dropped from the accounts struct | keep the field; `invoke` needs the program account even where Anchor's lints say it is removable |
| `InvalidProgramId` | a bundle injected or was overridden with the wrong program for a typed `Program<'info, T>` field | check the field's `T`; the conversion injects `<T as Id>::id()`. Overriding it past the type is the test's bug, not the derive's |
| `Stack offset of ... exceeded max offset` | an oversized stack frame in the program (large `Accounts` struct or big locals) | `Box` the large accounts in the program; this is a program bug the framework surfaced, not a harness limit |
| `Attempt to debit an account but found no record of a prior credit` | the signer was never funded | declare it as an actor (`ctx.cast_actor(name)` / `svm.actor(name, lamports)`) instead of a bare `Keypair::new()` |
| `Provided seeds do not result in a valid address` | `InvalidSeeds`: an account's address does not match the program's derivation (a non-derived PDA or ATA passed in) | pass the derived address; in a negative test, assert on this string directly. It is a builtin `ProgramError`, so `register_program_errors` does not apply |
| `InstructionError(_, Custom(2))` from a precompile (ed25519/secp256k1) | `PrecompileError::InvalidSignature`: the runtime rejected a forged or malformed signature instruction | expected on the forgery path of a signature audit; assert on `Custom(2)`. Precompile errors are a third naming category (not a program's custom error, not a builtin message); no registry names them |
| `deploy_from_file: ... bytes — likely an entrypoint-less stub` | same as `EntrypointOutOfBounds`, caught before deploy | same rebuild; the panic message names the path it read |
| `send_err_named` panic: error name mismatch | the program failed, but with a different error than the test expected | read the structured logs in the panic output; the failing frame renders `Name (0xcode)` when the error table is registered |

## Notes

- **`AlreadyProcessed` should not appear in helper-mediated suites.** Every
  `ctx.tx(..)` / `execute_instruction` send refreshes the blockhash first. If
  you see it, the send bypassed the helpers; prefer routing it through them
  over scattering `expire_blockhash` calls.
- **A sub-4-KiB `.so` is almost never a real program.** A plain
  `cargo build-sbf` against a crate whose `entrypoint!` sits behind a feature
  yields an ~896-byte shell. `deploy_from_file` rejects these with a
  diagnosis instead of letting the load fail later.
- **Error names come from registration; builtins come from the runtime.**
  Anchor programs get names decoded from logs automatically; Pinocchio
  programs register tables once via the derives
  (`register_program_instructions` / `register_program_errors`), and failed
  frames render `InvalidAmount (0x7)` at any CPI depth. Registration covers
  *custom* program errors only; a builtin `ProgramError` (`InvalidSeeds`,
  `IncorrectProgramId`, ...) renders as the runtime's own message, so
  negative tests assert on that string.
