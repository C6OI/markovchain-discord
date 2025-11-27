CREATE TABLE enabled_guilds(
    guild_id BIGINT NOT NULL,
    channel_id BIGINT NOT NULL,
    interval SMALLINT NULL,
    msgs_until_gen SMALLINT NOT NULL,
    CONSTRAINT guild_channel PRIMARY KEY (guild_id, channel_id)
);
