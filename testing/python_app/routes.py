from fastapi import APIRouter
from .models import get_users, User
from typing import List

router = APIRouter()

@router.get("/users", response_model=List[User])
def list_users():
    return get_users()
