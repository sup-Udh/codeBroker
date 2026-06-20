from utils.logger import get_logger

logger = get_logger(__name__)

def validate_email(email: str) -> bool:
    logger.info(f"Validating email {email}")
    return "@" in email
