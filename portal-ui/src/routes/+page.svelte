<script lang="ts">
    import { onMount } from "svelte";

    onMount(async() => {
        const result = await fetch("http://192.168.2.12:3033/.well-known/oauth-authorization-server");

        console.log("oauth-authorization-server", result);

        const body = document.createElement("div");

        body.classList.add("json-editor");

        for (const [key, val] of Object.entries(result)) {
            console.log(key, val);
        }
    })

</script>

<h1>Portal Unicore</h1>
<p>This is the portal to manage Unicore settings</p>

<h2>Issuer</h2>
TODO

<h2>OID4VC</h2>
OAUTH-authorization-server

openid-credential-issuer

POST: token
POST: credential
