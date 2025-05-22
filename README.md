# NEAR DevHub KYC Proxy Service

## Deploy

Install [`cargo-shuttle`](https://github.com/shuttle-hq/shuttle?tab=readme-ov-file#getting-started), create `Secret.toml` file with `AIRTABLE_API_TOKEN = ""` configured, and deploy with the following command:

```sh
$ cargo shuttle deploy
```

## Get sample Airtable data

The command below shows how to get the KYC data from the endpoint used by the proxy.

```bash
curl -H "Authorization: Bearer <AIRTABLE_API_TOKEN>" https://api.airtable.com/v0/<BASE_ID>/<TABLE_ID> -o kyc.json
```
