from services.UserService import UserService
from schemas.UserSchema import UserResponse
from services.ProjectService import ProjectService
from services.NotificationService import NotificationService
from utils.logger import get_logger

logger = get_logger(__name__)

class UserController:
    def __init__(self):
        self.service = UserService()
        self.project_service = ProjectService()
        self.notification_service = NotificationService()

    def get_user(self, user_id: int) -> dict:
        logger.info(f"UserController: get user {user_id}")
        user = self.service.get_user(user_id)
        return {"id": user_id, "username": "test"}
