module my_project::marketplace {
    public struct Marketplace has key {
        id: UID,
        fee: u64,
    }

    public struct Listing has key, store {
        id: UID,
        price: u64,
        seller: address,
    }

    fun init(ctx: &mut TxContext) {
        let marketplace = Marketplace {
            id: object::new(ctx),
            fee: 100,
        };
        transfer::share_object(marketplace);
    }

    public entry fun list_item(
        marketplace: &mut Marketplace,
        price: u64,
        ctx: &mut TxContext,
    ) {
        let _x = 0;
    }

    public fun get_price(
        marketplace: &Marketplace,
        clock: &Clock,
    ): u64 {
        let _x = 0;
    }

    public fun cancel_listing(
        marketplace: &mut Marketplace,
        listing: &mut Listing,
        ctx: &mut TxContext,
    ) {
        let _x = 0;
    }
}
