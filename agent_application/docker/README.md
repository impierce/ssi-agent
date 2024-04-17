# Docker

## Build the image

From within the directory `/agent_application` run:

```bash
docker build -f docker/Dockerfile -t ssi-agent ..
```

## Local development

Inside the folder `/agent_application/docker`:

1. Inside `docker-compose.yml` replace the environment value: `AGENT_APPLICATION_URL` with your actual local IP address or URL (such as http://192.168.1.234:3033)
2. Optionally, add the following environment variables:
   - `AGENT_ISSUANCE_CREDENTIAL_NAME`: To set the name of the credentials that will be issued.
   - `AGENT_ISSUANCE_CREDENTIAL_LOGO_URL`: To set the URL of the logo that will be used in the credentials.
> [!IMPORTANT] 
> 3. By default, UniCore currently uses a default Stronghold file which is used for storing secrets. Using this default
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
4. Optionally it is possible to configure an HTTP Event Publisher that can listen to certain events in `UniCore`
   and publish them to a `target_url`. More information about the HTTP Event Publisher can be found [here](../../agent_event_publisher_http/README.md).
5. To start the **SSI Agent**, a **Postgres** database along with **pgadmin** (Postgres Admin Interface) simply run:

```bash
docker compose up
```

6. The REST API will be served at `http://0.0.0.0:3033`

> [!NOTE]
> If you don't have rewrite rules enabled on your reverse proxy, you can set the `AGENT_CONFIG_BASE_PATH` to a value such as `ssi-agent`.
