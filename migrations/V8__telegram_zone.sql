-- The reader's time zone, so a message reads in the hours they keep. Kept as
-- the offset that formats it and the zone's name for the record; the page
-- refreshes both, which is also how a zone that shifts for summer stays right.
ALTER TABLE telegram_chat ADD COLUMN utc_offset INTEGER NOT NULL DEFAULT 0;
ALTER TABLE telegram_chat ADD COLUMN zone TEXT;
ALTER TABLE telegram_link ADD COLUMN utc_offset INTEGER NOT NULL DEFAULT 0;
ALTER TABLE telegram_link ADD COLUMN zone TEXT;
