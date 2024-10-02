![Banner](https://images.placeholders.dev/?width=1280&height=720)

Full HTTP API reference for the UniCore SSI Agent.

## Overview

### Management endpoints

### Standardized endpoints

Some endpoints that UniCore offers follow a specification (such as the [OpenID4VC](https://openid.net/sg/openid4vc/specifications) protocol family). These endpoints have the **`(standardized)`** tag.

### Public endpoints

Some endpoints should always be publicly accessible to allow identity wallets to interact with UniCore and follow standard protocol flows. These endpoints have the **`(public)`** tag.

> [!NOTE]
> Endpoints that should not sit behind some form of authentication are grouped under the `(public)` tag.

```json
{
  "foo": "bar"
}
```

## Authentication & Authorization

UniCore does not have any user management or authentication built-in (yet). It does not know of any roles or scopes. It is expected that the application which calls UniCore only performs calls which have been checked in the consumer business logic. If you want to deploy UniCore publicly, you should restrict access to the API by running it behind a reverse proxy or some API gateway. In most cases, only the endpoints behind `/v0` need to be protected, but all other endpoints should stay publicly accessible.

### Example reverse proxy configuration

Here is an example Nginx configuration that restricts access to the `/v0` endpoints by checking for a valid API key in the headers:

<details>
  <summary>nginx.conf</summary>

```
http {
    server {
        listen 8080;
        gzip on;

        location /v0 {
            if ($http_x_api_key != "A041FE585C6F45CF841D20D47D329FA5") {
                return 403;
            }

            proxy_pass http://127.0.0.1:3033/v0;
        }

        location / {
            proxy_pass http://127.0.0.1:3033;
        }
    }
}
```

</details>
