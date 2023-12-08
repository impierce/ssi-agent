# Docker

## Build the image

From within the directory `/agent_application` run:

```bash
docker build -f docker/Dockerfile -t ssi-agent ..
```

## Local development

Inside the folder `/agent_application/docker`:

1. _Inside `docker-compose.yaml` replace the value `<your-local-ip>` for the environment variable `AGENT_APPLICATION_HOST` with your actual local ip address (such as 192.168.1.234)_

2. To start the **SSI Agent**, a **Postgres** database along with **pgadmin** (Postgres Admin Interface) simply run:

```bash
docker compose up -d
```

3. The REST API will be served at `http://0.0.0.0:3033`
