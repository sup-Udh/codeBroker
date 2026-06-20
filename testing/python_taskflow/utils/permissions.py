from utils.logger import get_logger

logger = get_logger(__name__)

def check_permission(user_role: str, required_role: str) -> bool:
    logger.info(f"Checking permission {user_role} vs {required_role}")
    return user_role == required_role
