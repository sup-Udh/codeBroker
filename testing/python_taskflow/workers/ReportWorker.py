# Circular Dependency Participant: ReportWorker
from utils.logger import get_logger

logger = get_logger(__name__)

class ReportWorker:
    def process_report(self):
        logger.info("ReportWorker: Processing report")
        
    def log_notification(self, message: str):
        logger.info(f"ReportWorker: Logging notification '{message}'")
        # To complete circular dependency: ReportWorker -> ReportService
        from services.ReportService import ReportService
        report_service = ReportService()
        report_service.register_report_usage()
