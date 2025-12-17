# NEAR DevHub KYC Proxy Service

## Run

```sh
$ cargo run
```

## Deploy

This service is automatically deployed to Render.

## Get sample Airtable data

The command below shows how to get the KYC data from the endpoint used by the proxy.

```bash
curl -H "Authorization: Bearer <AIRTABLE_API_TOKEN>" https://api.airtable.com/v0/<BASE_ID>/<TABLE_ID> -o kyc.json
```
