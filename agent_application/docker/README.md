# Docker

## Build the image

From within the directory `/agent_application` run:

```bash
docker build -f docker/Dockerfile -t ssi-agent ..
```

## Local development

Inside the folder `/agent_application/docker`:

1. _Inside `docker-compose.yml` replace the value `<your-local-ip>` for the environment variable `AGENT_APPLICATION_HOST` with your actual local ip address (such as 192.168.1.234)_
2. Optionally, add the following environment variables:
    - `AGENT_ISSUANCE_CREDENTIAL_NAME`: To set the name of the credentials that will be issued.
    - `AGENT_ISSUANCE_CREDENTIAL_LOGO_URL`: To set the URL of the logo that will be used in the credentials.
3. To start the **SSI Agent**, a **Postgres** database along with **pgadmin** (Postgres Admin Interface) simply run:

```bash
docker compose up -d
```


3. The REST API will be served at `http://0.0.0.0:3033`

~~~
**NOTE**

When you set the AGENT_RELATIVE_PATH to for example: "unicore"
it will be available at: `http://0.0.0.0:3033/unicore`

~~~
