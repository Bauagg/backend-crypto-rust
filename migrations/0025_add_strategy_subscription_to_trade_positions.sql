-- Posisi yang dibuka bot strategi user: milik pemakaian (subscription) mana. Dengan kolom ini tiap
-- bot hanya mengelola posisinya sendiri — tidak menjual coin robot Python (rule_type ROBOT), coin
-- manual, atau coin bot strategi lain milik user yang sama. Performa per versi tetap lewat
-- rule_type = 'STRATEGY' + rule_id = strategy_versions.id.
ALTER TABLE trade_positions
    ADD COLUMN strategy_subscription_id UUID REFERENCES strategy_subscriptions(id);

CREATE INDEX idx_trade_positions_subscription ON trade_positions (strategy_subscription_id, status)
    WHERE strategy_subscription_id IS NOT NULL;
