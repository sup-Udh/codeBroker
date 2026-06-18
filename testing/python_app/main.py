from fastapi import FastAPI
from .routes import router
from .models import init_db

app = FastAPI(title="Sample Python App")

@app.on_event("startup")
def on_startup():
    init_db()

app.include_router(router, prefix="/api/v1")

@app.get("/")
def read_root():
    return {"status": "ok", "message": "Welcome to the Python API!"}
