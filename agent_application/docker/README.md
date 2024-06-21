# Docker

## Build the image

From within the directory `/agent_application` run:

```bash
docker build -f docker/Dockerfile -t ssi-agent ..
```

## Local development

Inside the folder `/agent_application/docker`:

1. Inside `docker-compose.yml` replace the environment value: `AGENT_APPLICATION_URL` with your actual local IP address or URL (such as http://192.168.1.234:3033)
> [!IMPORTANT] 
> 2. By default, UniCore currently uses a default Stronghold file which is used for storing secrets. Using this default
>    Stronghold is for testing purposes only and should not be used in production. To use your own Stronghold file, you
>    need to mount it in the `docker-compose.yml` file by replacing the default volume. Example:
> ```yaml
>  volumes:
>    # - ../../agent_secret_manager/tests/res/test.stronghold:/app/res/stronghold # Default Stronghold file
>    - /path/to/stronghold:/app/res/stronghold
>  ```
>    It is recommended to not change the target path `/app/res/stronghold`.
> 
>   You will also need to set the following environment variables: 
>   - `AGENT_SECRET_MANAGER_STRONGHOLD_PATH`: The path to the Stronghold file. This value must correspond to the path to which
>     the Stronghold is mounted. Set to `/app/res/stronghold` by default. It
>     is recommended to not change this environment variable.
>   - `AGENT_SECRET_MANAGER_STRONGHOLD_PASSWORD`: To set the password
>   - `AGENT_SECRET_MANAGER_ISSUER_KEY_ID`: To set the key id
3. Optionally it is possible to configure an HTTP Event Publisher that can listen to certain events in `UniCore`
   and publish them to a `target_url`. More information about the HTTP Event Publisher can be found [here](../../agent_event_publisher_http/README.md).
4. To start the **SSI Agent**, a **Postgres** database along with **pgadmin** (Postgres Admin Interface) simply run:

```bash
docker compose up
```

5. The REST API will be served at `http://0.0.0.0:3033`

> [!NOTE]
> If you don't have rewrite rules enabled on your reverse proxy, you can set the `AGENT_CONFIG_BASE_PATH` to a value such as `ssi-agent`.

## Utilizing the IOTA DID Method
By default, UniCore uses the JWK DID Method to generate and manage DIDs. However, UniCore also supports the IOTA DID
Method, which leverages the IOTA Tangle to store your DID document. To enable the IOTA DID Method, set these environment
variables:
```yaml
      AGENT_CONFIG_ISSUER_DID: <your-pre-existing-IOTA-DID>
      AGENT_CONFIG_ISSUER_FRAGMENT: <your-pre-existing-IOTA-DID-fragment>
```

and make sure to configure the `agent_application.config.yml` file so that the first item in the
`subject_syntax_types_supported` sequence is `did:iota:rms`.

UniCore supports any of the IOTA networks (Testnet, Shimmer, Mainnet). For example, if you want to enable the development network for Shimmer, the 
aforementioned environment variables would look like this:
```yaml
      AGENT_CONFIG_ISSUER_DID: "did:iota:rms:0x42ad588322e58b3c07aa39e4948d021ee17ecb5747915e9e1f35f028d7ecaf90"
      AGENT_CONFIG_ISSUER_FRAGMENT: "bQKQRzaop7CgEvqVq8UlgLGsdF-R-hnLFkKFZqW2VN0"
      AGENT_CONFIG_DEFAULT_DID_METHOD: "did:iota:rms"
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

   offer: {
      target_url: *target_url,
      target_events: [
         CredentialRequestVerified
      ]
   }
   ```
2. Upon initiation of the OpenID4VCI flow by a Wallet, the CredentialRequestVerified event is triggered, containing relevant identifiers.
3. The HTTP Event Publisher dispatches the event to the external system. Leveraging the provided identifiers, the external system generates and signs the credential, then submits it to UniCore's `/v0/credentials` endpoint. Refer to the [API specification](../../agent_api_rest/README.md)) for additional details on endpoint usage.

By default, UniCore will wait up to 1000 ms for the signed credential to arrive. This parameter can be changed by
setting the `AGENT_API_REST_EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS` environment variable.
