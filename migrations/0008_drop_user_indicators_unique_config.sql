-- User boleh punya beberapa baris untuk indicator_type yang sama (mis. SMA period 50 dan
-- SMA period 200 sekaligus) -- dibedakan lewat isi params, bukan dibatasi satu per indikator.
ALTER TABLE user_indicators DROP CONSTRAINT user_indicators_unique_config;
