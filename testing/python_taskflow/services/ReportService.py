# Circular Dependency Participant: ReportService
from services.ProjectService import ProjectService
from utils.logger import get_logger

logger = get_logger(__name__)

class ReportService:
    def generate_report(self) -> str:
        logger.info("ReportService: Generating report")
        
        # To complete circular dependency: ReportService -> NotificationService
        from services.NotificationService import NotificationService
        notification_service = NotificationService()
        notification_service.send_notification(1, "Report generated")
        
        project_service = ProjectService()
        project_service.get_project(1)
        
        return "Report content"

    def register_report_usage(self):
        logger.info("ReportService: Registering usage")
