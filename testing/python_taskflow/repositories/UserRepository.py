from models.User import User
from utils.logger import get_logger

logger = get_logger(__name__)

class UserRepository:
    def __init__(self):
        self.db = []
    
    def get_by_id(self, user_id: int) -> User | None:
        logger.info(f"UserRepository fetching user {user_id}")
        return next((u for u in self.db if u.id == user_id), None)
