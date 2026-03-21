module my_project::marketplace_events {
    // Event structs — copy+drop, not used as function params
    public struct ItemPurchased has copy, drop {
        buyer: address,
        seller: address,
        price: u64,
        item_id: address,
    }

    public struct ListingCreated has copy, drop {
        seller: address,
        price: u64,
        listing_id: address,
    }

    public struct FeeCollected has copy, drop {
        amount: u64,
        recipient: address,
    }

    // This is a value struct used as a function param — NOT an event
    public struct PriceRange has copy, drop {
        min_price: u64,
        max_price: u64,
    }

    // On-chain object — NOT an event
    public struct Marketplace has key {
        id: UID,
        fee_bps: u16,
    }

    fun init(ctx: &mut TxContext) {
        let marketplace = Marketplace {
            id: object::new(ctx),
            fee_bps: 250,
        };
        transfer::share_object(marketplace);
    }

    public fun purchase_item(
        marketplace: &mut Marketplace,
        price: u64,
        ctx: &mut TxContext,
    ) {
        abort 0
    }

    // This function uses PriceRange as a param — so PriceRange is NOT an event
    public fun search_listings(
        marketplace: &Marketplace,
        range: PriceRange,
        ctx: &mut TxContext,
    ) {
        abort 0
    }
}
