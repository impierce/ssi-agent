### 24-01-2024

Environment variable `AGENT_APPLICATION_HOST` is changed to `AGENT_APPLICATION_URL` and requires the complete URL. e.g.: 
`https://my.domain.com/unicore`. In case you don't have rewrite root enabled on your reverse proxy, you will have to set `AGENT_APPLICATION_BASE_PATH` as well. e.g.: `unicore`.
