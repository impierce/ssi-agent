# agent_api_rest

A lightweight REST API for the SSI Agent.

### OpenAPI specification (Swagger UI)

```bash
docker run --rm -p 9090:8080 -e SWAGGER_JSON=/tmp/openapi.yaml -v $(pwd):/tmp swaggerapi/swagger-ui
```

Browse to http://localhost:9090

### CORS

If you want to access UniCore's API from a browser, you can set the `AGENT_APPLICATION_ENABLE_CORS` environment variable to `true`. This will enable a permissive CORS policy (allow all).
