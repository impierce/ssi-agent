# Docker

## Build the image

In case you want to build the image in isolation, you can run the following command from within the directory `/agent_application`:

```bash
docker build -f docker/Dockerfile -t ssi-agent ..
```

## Local development

For local development, it is recommended to use Docker Compose.

1. Set the environment variable `UNICORE__URL` to the following pattern: `http://<your-local-ip>:3033`, so it looks something like `http://192.168.1.100:3033`. You can copy `docker/.env.example` to `docker/.env` and adjust the value there.
2. A Stronghold secret file is generated inside the container at the path defined in `UNICORE__SECRET_MANAGER__STRONGHOLD_PATH` and destroyed when the container is destroyed.
   If you have an existing file or you want to reuse a Stronghold file, you can mount it under `volumes:` and set the environment variable `UNICORE__SECRET_MANAGER__STRONGHOLD_PATH` to the path where the Stronghold file is mounted.
   An example could look like this:

```yaml
environment:
  UNICORE__SECRET_MANAGER__STRONGHOLD_PATH: "/app/res/stronghold"

volumes:
  - ../../agent_secret_manager/tests/res/test.stronghold:/app/res/stronghold
```

3. _(optional)_ In case you are interested in the events that UniCore produces, you can configure a HTTP Event Publisher that sends
   certain events to a URL of your choice. More information about the HTTP Event Publisher [can be found here](../../agent_event_publisher_http/README.md).

4. To start the **SSI Agent**, a **Postgres** database along with **pgadmin** (Postgres Admin Interface) simply run:

```bash
docker compose up
```

5. The REST API will be served at the value you set in `UNICORE__URL` (and also at `http://0.0.0.0:3033`).

> [!NOTE]
> In case you need a base bath (for example when running behind a reverse proxy), you can set the `UNICORE__BASE_PATH` to a value such as `ssi-agent`.

## IOTA DIDs

By default, UniCore uses the JWK DID Method to generate and manage DIDs. However, UniCore also supports the IOTA DID
Method, which leverages the IOTA Tangle to store your DID document. To enable the IOTA DID Method, set these environment
variables:

```yaml
UNICORE__SECRET_MANAGER__ISSUER_DID: <your-pre-existing-IOTA-DID>
UNICORE__SECRET_MANAGER__ISSUER_FRAGMENT: <your-pre-existing-IOTA-DID-fragment>
```
