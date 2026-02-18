# Please make sure Node.js >= 18 is installed.

# 1. Install Bruno CLI
# npm install -g @usebruno/cli

# 2. Remove existing directory if it exists
rm -r ./UniCore\ HTTP\ API

# 3. Import OpenAPI specification
bru import openapi \
  --source ../openapi-generated.yaml \
  --output ./

cd ./UniCore\ HTTP\ API

mkdir environments

# 4. (experimental) Manually add environment for local testing
echo "vars {
  baseUrl: http://127.0.0.1:3033
}" > ./environments/localhost.bru

# 5. (experimental) Manually add post-response script to save the template ID into a runtime variable.
echo "vars:post-response {
  id: res.body.id
}

script:post-response {
  console.log(res.body.id)
}" >> ./library/Create\ a\ new\ template.bru

# 6. (experimental) Manually add assertion to response
echo "assert {
  res.body.title: endsWith " Copy2"
}" >> ./library/Duplicate\ existing\ template.bru

# 7. Run the collection
bru run --env-file ./environments/localhost.bru --reporter-json results.json --reporter-html results.html
