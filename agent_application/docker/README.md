# Docker

## Local development

SSI Agent + Postgres + Admin UI

```bash
docker compose up -d
```

Go to `http://localhost:5433` to access the event store database.

## Build

From within the `/agent_application` directory run:

```bash
docker build -f docker/Dockerfile -t ssi-agent ..
```
