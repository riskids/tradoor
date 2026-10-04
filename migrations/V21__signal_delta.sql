-- The delta at the drives, where the exchange reports who took each trade:
-- whether buyers weakened from the push into the second drive to the push into
-- the third, and whether the third drive's own bar was sold more than bought.
ALTER TABLE signal
    ADD COLUMN delta_divergence BOOLEAN,
    ADD COLUMN delta_against    BOOLEAN;
ALTER TABLE signal_backtest
    ADD COLUMN delta_divergence BOOLEAN,
    ADD COLUMN delta_against    BOOLEAN;
