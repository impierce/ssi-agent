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

## Leveraging Just-in-Time Data Request Events

UniCore facilitates dynamic integration with external systems through just-in-time data request events, dispatched seamlessly via an HTTP Event Publisher. This enables real-time data retrieval and on-demand generation, enhancing flexibility and efficiency in your SSI ecosystem.

### Example Scenarios

**Custom Credential Signing**

UniCore facilitates the utilization of just-in-time data request events for customized credential signing workflows. This approach enables users to manage the signing process independently, offering greater control over credential issuance. When UniCore verifies a Credential Request from a Wallet, it triggers the `CredentialRequestVerified` event. By utilizing the HTTP Event Publisher, this event, containing essential identifiers like `offer_id` and `subject_id`, can be dispatched to external systems. Subsequently, external systems leverage these identifiers to generate and sign credentials, which are then submitted to UniCore's `/v0/credentials` endpoint.

To integrate just-in-time data request events into your workflow, adhere to the following steps:

1. Configure the HTTP Event Publisher to listen for the `CredentialRequestVerified` event. Refer to the [HTTP Event Publisher documentation](../../agent_event_publisher_http/README.md) for detailed configuration instructions:

   ```yaml
   target_url: &target_url "https://my-domain.example.org/ssi-event-subscriber"

   offer:
     { target_url: *target_url, target_events: [CredentialRequestVerified] }
   ```

2. Upon initiation of the OpenID4VCI flow by a Wallet, the CredentialRequestVerified event is triggered, containing relevant identifiers.
3. The HTTP Event Publisher dispatches the event to the external system. Leveraging the provided identifiers, the external system generates and signs the credential, then submits it to UniCore's `/v0/credentials` endpoint. Refer to the [API specification](../../agent_api_rest/README.md)) for additional details on endpoint usage.

By default, UniCore will wait up to 1000 ms for the signed credential to arrive. This parameter can be changed by
setting the `AGENT_API_REST_EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS` environment variable.
