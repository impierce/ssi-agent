# Pipeline

In order to run the pipeline build script locally, create a `.env` file in `.github/.pipeline` and add the following content:

```sh
IMAGE=unicore
ARTIFACT_REGISTRY_HOST=<ask-the-repository-owner>
ARTIFACT_REGISTRY_REPOSITORY=<ask-the-repository-owner>
PROJECT_ID=<ask-the-repository-owner>
GITHUB_SHA=test_sha
APISIX_PATH=unicore
```

Then execute `./build.sh`.
