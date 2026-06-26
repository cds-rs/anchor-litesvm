# The Scandals

By now you have a cast of characters, a stage, and a script. The interesting tests are not the ones where everyone behaves. They are the ones where somebody lies, arrives without authority, signs the wrong document, or tries to spend money they don't control. In other words: the scandals.

That framing is the suggestion of this part, and it's a writing technique more than a feature. Write each test as a scene. The happy path is the deal going through; the failures are the scenes where it shouldn't. Give each one the name of its human intent, not just its error code:

- **The Betrayal**: someone acts outside the terms they agreed to (a maker refunding before the window closes).
- **The Impostor**: someone wields an authority they were never granted.
- **The No-Show**: a required signer never turns up.
- **Mistaken Identity**: the wrong PDA stands in for the real one.
- **The Forgery**: an account owned by the wrong program is passed off as genuine.
- **The Double Cross**: an authority abuses a legitimate power against the people it was meant to protect (an admin trading through a locked pool).

Naming the *intent*, not the error code, is what makes a test memorable to write and obvious to read six months later.

The framework is built to reward the stance. A failed transaction renders in the names you cast, so the scene reads like a story:

<div class="callout scandal">

Maker attempts to refund the escrow.
Escrow rejects it: `EscrowNotExpired`.

</div>

instead of the very same failure left unaliased:

> `4N6…` attempts an instruction.
> `9Lm…` rejects it: `custom program error: 0x1771`.

You debug in terms of intent and responsibility, not addresses and account indices. And there is a discipline under the drama: a scandal is only *proven* if the right guard fires, which is why these tests assert the specific error with [`send_err_named`](../running/executing.md), not merely that something failed.

> **Happy paths prove the system works. Scandals prove it fails for the right reasons.**

<div class="callout spotlight">

These pages mark the scenes with the diagrams' own encoding (see [Part IV](../inspect/cpi-tree.md)): red flags the failure under study, blue spotlights a subtlety worth a second look, and everything routine stays unmarked.

</div>

The three examples that follow each open with their cast, run the deal, then stage their scandals: Vault guards its seeds (Mistaken Identity), Escrow honors its clock (The Betrayal), and the AMM survives an authority who tries to trade through a locked pool (The Double Cross).
