export async function fetchOAuthServer() {
    const result = await fetch("http://192.168.2.12:3033/.well-known/oauth-authorization-server");
    
    console.log("oauth-authorization-server", result);

    createJsonEditor(result);

}

export function createJsonEditor(data: any) {
    const body = new HTMLDivElement();
    body.classList.add("json-editor");

    for (const [key, val] of Object.entries(data)) {
        console.log(key, val);
    }
}
