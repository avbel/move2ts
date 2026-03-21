module my_project::marketplace_events {
    use sui::event;

    // Event structs — copy+drop, emitted via event::emit()
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

    // This is a value struct used as a function param AND emitted — tests Event suffix
    public struct TradeInfo has copy, drop {
        trade_amount: u64,
        trade_fee: u64,
    }

    // This is a value struct used ONLY as a function param — NOT an event
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
        event::emit(ItemPurchased {
            buyer: ctx.sender(),
            seller: ctx.sender(),
            price,
            item_id: ctx.sender(),
        });
        event::emit(FeeCollected {
            amount: price / 100,
            recipient: ctx.sender(),
        });
    }

    public fun create_listing(
        marketplace: &mut Marketplace,
        price: u64,
        ctx: &mut TxContext,
    ) {
        event::emit(ListingCreated {
            seller: ctx.sender(),
            price,
            listing_id: ctx.sender(),
        });
    }

    // This function emits TradeInfo AND uses it as a param
    public fun execute_trade(
        marketplace: &mut Marketplace,
        info: TradeInfo,
        ctx: &mut TxContext,
    ) {
        event::emit(TradeInfo {
            trade_amount: info.trade_amount,
            trade_fee: info.trade_fee,
        });
    }

    // This function uses PriceRange as a param but does NOT emit it
    public fun search_listings(
        marketplace: &Marketplace,
        range: PriceRange,
        ctx: &mut TxContext,
    ) {
        abort 0
    }
}
