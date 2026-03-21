module my_project::defi {
    public struct Pool has key {
        id: UID,
        balance: u64,
    }

    public fun swap<X, Y>(
        pool: &mut Pool,
        amount_in: u64,
        min_out: u64,
        ctx: &mut TxContext,
    ): u64 {
        abort 0
    }

    public fun withdraw<T>(
        pool: &mut Pool,
        amount: u64,
        ctx: &mut TxContext,
    ) {
        abort 0
    }

    public fun get_random_reward(
        pool: &mut Pool,
        rng: &Random,
        clock: &Clock,
        ctx: &mut TxContext,
    ): u64 {
        abort 0
    }
}
