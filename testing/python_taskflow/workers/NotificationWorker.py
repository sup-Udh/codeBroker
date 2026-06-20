from services.NotificationService import NotificationService
from services.ProjectService import ProjectService
from utils.logger import get_logger

logger = get_logger(__name__)

class NotificationWorker:
    def run(self):
        logger.info("NotificationWorker: Running")
        notif_service = NotificationService()
        notif_service.send_notification(1, "Worker started")
        
        proj_service = ProjectService()
        proj_service.get_project(1)
