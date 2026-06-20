from services.ReportService import ReportService
from services.ProjectService import ProjectService
from services.NotificationService import NotificationService
from utils.logger import get_logger

logger = get_logger(__name__)

class ReportController:
    def __init__(self):
        self.service = ReportService()
        self.project_service = ProjectService()
        self.notification_service = NotificationService()

    def generate_report(self) -> dict:
        logger.info("ReportController: generating report")
        report = self.service.generate_report()
        return {"report": report}
