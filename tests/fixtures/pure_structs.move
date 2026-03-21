module my_project::config {
    public struct Config has copy, drop {
        max_items: u64,
        fee_bps: u16,
        enabled: bool,
    }

    public struct Metadata has copy, drop {
        name: vector<u8>,
        version: u32,
    }

    public struct Registry has key {
        id: UID,
        config: Config,
    }

    fun init(ctx: &mut TxContext) {
        let registry = Registry {
            id: object::new(ctx),
            config: Config {
                max_items: 100,
                fee_bps: 250,
                enabled: true,
            },
        };
        transfer::share_object(registry);
    }

    public fun update_config(
        registry: &mut Registry,
        new_config: Config,
        ctx: &mut TxContext,
    ) {
        abort 0
    }

    public fun set_metadata(
        registry: &mut Registry,
        metadata: Metadata,
        clock: &Clock,
        ctx: &mut TxContext,
    ) {
        abort 0
    }

    public fun get_config(registry: &Registry): Config {
        abort 0
    }

    public entry fun apply_defaults(
        registry: &mut Registry,
        max_items: u64,
        fee_bps: u16,
        ctx: &mut TxContext,
    ) {
        abort 0
    }
}
