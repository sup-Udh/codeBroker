# Circular Dependency Participant: NotificationService
from utils.logger import get_logger

logger = get_logger(__name__)

class NotificationService:
    def send_notification(self, user_id: int, message: str) -> None:
        logger.info(f"NotificationService: Sending '{message}' to user {user_id}")
        # To complete circular dependency: NotificationService -> ReportWorker
        from workers.ReportWorker import ReportWorker
        worker = ReportWorker()
        worker.log_notification(message)
