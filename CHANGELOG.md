### 23-04-2024
Renamed `subjectId` to `offerId`. This has effect on both the `/v1/credentials` and `/v2/offers` endpoints.

The `/v1/credentials` endpoint now accepts an object or a string as the `credential` value (previously it accepted only
objects). It also accepts an optional `isSigned` parameter, which indicates that the credential is already signed and
does not need to be signed in UniCore.

### 11-04-2024
`/v1/offers` incorrectly returned with Content-Type `application/json`. The Content-Type has now been changed to `application/x-www-form-urlencoded`.

### 24-01-2024

Environment variable `AGENT_APPLICATION_HOST` has changed to `AGENT_APPLICATION_URL` and requires the complete URL. e.g.:
`https://my.domain.com/unicore`. In case you don't have rewrite root enabled on your reverse proxy, you will have to set `AGENT_CONFIG_BASE_PATH` as well. e.g.: `unicore`.
