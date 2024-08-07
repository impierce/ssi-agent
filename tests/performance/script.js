import http from "k6/http";
import { sleep } from "k6";
import { uuidv4 } from "https://jslib.k6.io/k6-utils/1.4.0/index.js";

const HOST = "http://192.168.1.234:3033";

export const options = {
  // A number specifying the number of VUs to run concurrently.
  vus: 10,
  // A string specifying the total duration of the test run.
  duration: "30s",
};

function createCredential() {
  const url = `${HOST}/v0/credentials`;

  const offerId = uuidv4();

  const payload = JSON.stringify({
    offerId,
    credentialConfigurationId: "w3c_vc_credential",
    credential: {
      credentialSubject: {
        first_name: "Ferris",
        last_name: "Crabman",
        dob: "1982-01-01",
      },
    },
  });

  const params = {
    headers: {
      "Content-Type": "application/json",
    },
  };

  http.post(url, payload, params);
}

export default function () {
  //   http.get(`${HOST}/.well-known/openid-credential-issuer`);
  createCredential();
  sleep(1);
}
