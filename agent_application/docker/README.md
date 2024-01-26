# Docker

## Build the image

From within the directory `/agent_application` run:

```bash
docker build -f docker/Dockerfile -t ssi-agent ..
```

## Local development

Inside the folder `/agent_application/docker`:

1. Inside `docker-compose.yml` replace the environment value: `AGENT_APPLICATION_URL` with your actual local ip address or url (such as http://192.168.1.234:3033)
2. Optionally, add the following environment variables:
   - `AGENT_ISSUANCE_CREDENTIAL_NAME`: To set the name of the credentials that will be issued.
   - `AGENT_ISSUANCE_CREDENTIAL_LOGO_URL`: To set the URL of the logo that will be used in the credentials.
3. To start the **SSI Agent**, a **Postgres** database along with **pgadmin** (Postgres Admin Interface) simply run:

```bash
docker compose up
```

4. The REST API will be served at `http://0.0.0.0:3033`

> [!NOTE]
> If you don't have rewrite rules enabled on your reverse proxy, you can set the `AGENT_CONFIG_BASE_PATH` to a value such as `ssi-agent`.
