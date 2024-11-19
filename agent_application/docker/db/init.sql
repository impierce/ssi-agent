CREATE TABLE events
(
    aggregate_type text                         NOT NULL,
    aggregate_id   text                         NOT NULL,
    sequence       bigint CHECK (sequence >= 0) NOT NULL,
    event_type     text                         NOT NULL,
    event_version  text                         NOT NULL,
    payload        json                         NOT NULL,
    metadata       json                         NOT NULL,
    PRIMARY KEY (aggregate_type, aggregate_id, sequence)
);

CREATE TABLE offer
(
    view_id           text                        NOT NULL,
    version           bigint CHECK (version >= 0) NOT NULL,
    payload           json                        NOT NULL,
    PRIMARY KEY (view_id)
);

CREATE TABLE all_offers
(
    view_id           text                        NOT NULL,
    version           bigint CHECK (version >= 0) NOT NULL,
    payload           json                        NOT NULL,
    PRIMARY KEY (view_id)
);

CREATE TABLE pre_authorized_code
(
    view_id           text                        NOT NULL,
    version           bigint CHECK (version >= 0) NOT NULL,
    payload           json                        NOT NULL,
    PRIMARY KEY (view_id)
);

CREATE TABLE access_token
(
    view_id           text                        NOT NULL,
    version           bigint CHECK (version >= 0) NOT NULL,
    payload           json                        NOT NULL,
    PRIMARY KEY (view_id)
);

CREATE TABLE credential
(
    view_id           text                        NOT NULL,
    version           bigint CHECK (version >= 0) NOT NULL,
    payload           json                        NOT NULL,
    PRIMARY KEY (view_id)
);

CREATE TABLE all_credentials
(
    view_id           text                        NOT NULL,
    version           bigint CHECK (version >= 0) NOT NULL,
    payload           json                        NOT NULL,
    PRIMARY KEY (view_id)
);

CREATE TABLE server_config
(
    view_id           text                        NOT NULL,
    version           bigint CHECK (version >= 0) NOT NULL,
    payload           json                        NOT NULL,
    PRIMARY KEY (view_id)
);

CREATE TABLE received_offer
(
    view_id           text                        NOT NULL,
    version           bigint CHECK (version >= 0) NOT NULL,
    payload           json                        NOT NULL,
    PRIMARY KEY (view_id)
);

CREATE TABLE all_received_offers
(
    view_id           text                        NOT NULL,
    version           bigint CHECK (version >= 0) NOT NULL,
    payload           json                        NOT NULL,
    PRIMARY KEY (view_id)
);

CREATE TABLE holder_credential
(
    view_id           text                        NOT NULL,
    version           bigint CHECK (version >= 0) NOT NULL,
    payload           json                        NOT NULL,
    PRIMARY KEY (view_id)
);


CREATE TABLE all_holder_credentials
(
    view_id           text                        NOT NULL,
    version           bigint CHECK (version >= 0) NOT NULL,
    payload           json                        NOT NULL,
    PRIMARY KEY (view_id)
);

CREATE TABLE authorization_request
(
    view_id           text                        NOT NULL,
    version           bigint CHECK (version >= 0) NOT NULL,
    payload           json                        NOT NULL,
    PRIMARY KEY (view_id)
);

CREATE TABLE connection
(
    view_id           text                        NOT NULL,
    version           bigint CHECK (version >= 0) NOT NULL,
    payload           json                        NOT NULL,
    PRIMARY KEY (view_id)
);

CREATE USER demo_user WITH ENCRYPTED PASSWORD 'demo_pass';
GRANT ALL PRIVILEGES ON DATABASE postgres TO demo_user;
