# Docker

## Local development

SSI Agent + Postgres + Admin UI

```bash
docker compose up -d
```

Go to `http://localhost:5433` to access the event store database.

## Deployment

```bash
docker build -t ssi-agent .
```

### Run

```bash
docker run --rm -p 3033:3033 ssi-agent
```
