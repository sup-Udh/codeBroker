from fastapi import APIRouter, Depends
from app.config.settings import settings

router = APIRouter(prefix="/admin", tags=["Admin"])

@router.get("/config")
def get_config():
    # Demonstrates depending on config
    return {"db": settings.DATABASE_URL}
