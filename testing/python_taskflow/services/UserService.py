from repositories.UserRepository import UserRepository
from services.ProjectService import ProjectService
from services.NotificationService import NotificationService
from utils.logger import get_logger

logger = get_logger(__name__)

class UserService:
    def __init__(self):
        self.repo = UserRepository()
        self.project_service = ProjectService()
        self.notification_service = NotificationService()
    
    def get_user(self, user_id: int):
        logger.info(f"UserService: Getting user {user_id}")
        self.notification_service.send_notification(user_id, "User accessed")
        return self.repo.get_by_id(user_id)
