from fastapi import APIRouter
from controllers.UserController import UserController

router = APIRouter()
controller = UserController()

@router.get("/users/{user_id}")
def get_user(user_id: int):
    return controller.get_user(user_id)
