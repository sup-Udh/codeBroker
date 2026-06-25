import os
import requests

key = os.environ.get("OPENAI_API_KEY", "")
print("Key length:", len(key))

resp = requests.post(
    "https://api.openai.com/v1/embeddings",
    headers={"Authorization": f"Bearer {key}"},
    json={"input": "Auth system", "model": "text-embedding-3-small"}
)
print("Status:", resp.status_code)
if resp.status_code != 200:
    print("Error:", resp.text)
