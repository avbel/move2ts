module my_project::maps {
    use sui::vec_map::VecMap;

    public struct Settings has copy, drop {
        labels: VecMap<u64, bool>,
        name: vector<u8>,
    }

    public fun set_labels(
        store: &mut UID,
        labels: VecMap<u64, bool>,
        ctx: &mut TxContext,
    ) {
        let _x = 0;
    }

    public fun update_settings(
        store: &mut UID,
        settings: Settings,
        ctx: &mut TxContext,
    ) {
        let _x = 0;
    }

    public fun set_addresses(
        store: &mut UID,
        targets: VecMap<address, u64>,
        ctx: &mut TxContext,
    ) {
        let _x = 0;
    }
}
