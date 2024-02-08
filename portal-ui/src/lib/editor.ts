export async function fetchOAuthServer(): Promise<Response> {
    const result = await fetch("http://192.168.2.12:3033/.well-known/oauth-authorization-server");

    console.log("oauth-authorization-server", result);

    return result;
}

