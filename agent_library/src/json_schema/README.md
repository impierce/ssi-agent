This folder contains the JSON Schemas of the digital credential data formats supported by UniMe:

- [Verifiable Credentials Data Model v1.1](https://www.w3.org/TR/vc-data-model-1.1)
  JSON Schema source: Created by us, Impierce Technologies, based on the specification
- [Verifiable Credentials Data Model v2.0](https://www.w3.org/TR/vc-data-model/)
  JSON Schema source: https://github.com/w3c/vc-data-model/blob/main/schema/verifiable-credential/verifiable-credential-schema.json, commit = "e45b60c"
- [Open Badges Specification 3.0](https://www.imsglobal.org/spec/ob/v3p0)
  JSON Schema source: https://www.imsglobal.org/spec/ob/v3p0#achievementcredential-0
- [European Digital Credential](https://op.europa.eu/en/web/eu-vocabularies/dataset/-/resource?uri=http://publications.europa.eu/resource/dataset/snb-model)
  JSON Schema source: https://op.europa.eu/en/web/eu-vocabularies/dataset/-/resource?uri=http://publications.europa.eu/resource/dataset/snb-model
  - [Verifiable Credentials Data Model v1.1](https://op.europa.eu/en/web/eu-vocabularies/dataset/-/resource?uri=http://publications.europa.eu/resource/dataset/snb-model) EDC builds upon the VC DM 1.1 but on a different JSON Schema as defined by the European Publication Office: https://op.europa.eu/en/web/eu-vocabularies/dataset/-/resource?uri=http://publications.europa.eu/resource/dataset/snb-model



*Important Notes:*
- We cannot issue truly official ELM credentials until the following points have been resolved
  - “National authorities” have issued an eIDAS legal identifier to the organization that wants to issue ELM credentials:
    https://europa.eu/europass/elm-browser/documentation/3-2-0/rdf/ap/edc/documentation/edc-generic-no-cv_en.html#edcgn:IssuerNodeShape 
  - An official ELM credential needs an E-Seal issued by a Trust Service Provider (TSP). The EDC issuer doesn't explain whether the E-Seal goes in the `proof` field or if it's an enveloppe.
  https://europass.europa.eu/en/how-issue-european-digital-credentials-learning#9379
  - An official ELM also preferes to have all images to be attached to the credential to be baked in, requiring at least one such baked in image. Without it the ELM is not valid.
  https://europa.eu/europass/elm-browser/documentation/3-2-0/rdf/ap/edc/documentation/edc-generic-no-cv_en.html#edcgn:MediaObjectShape
- The official location/resource of the Open Badges v3.0 JSON Schema is actually the link below which is not the same as the official specification URL:
https://purl.imsglobal.org/spec/ob/v3p0/schema/json/ob_v3p0_achievementcredential_schema.json



*Useful Resources:*
- [ELM publication office](https://op.europa.eu/en/web/eu-vocabularies/dataset/-/resource?uri=http://publications.europa.eu/resource/dataset/snb-model)
- [Introduction to ELM](https://europa.eu/europass/elm-browser/index.html#ontology)
- [ELM Ontology browser](https://europa.eu/europass/elm-browser/homepage/3-2-0/index-en.html)
- [ELM RDF Ontology](https://europa.eu/europass/elm-browser/documentation/3-2-0/rdf/ontology/documentation/index-en.html#/)
- [ELM SHACLE Ontology](https://europa.eu/europass/elm-browser/homepage/3-2-0/edc-generic-no-cv_en.html)
- [More ELM SHACLE Ontology](https://europa.eu/europass/elm-browser/homepage/3-2-0/edc-generic-full_en.html)
- [ELM examples](https://github.com/european-commission-empl/European-Learning-Model/tree/master/Credentials/JSON-LD%20Examples%20(ELM%20v3))
- [More ELM examples in another source control platform](https://code.europa.eu/qualifications-courses-and-credentials/ELM-support/-/tree/main/credential%20examples/JSON-LD%20Examples%20(ELM%20v3))