from models.Project import Project
from utils.logger import get_logger

logger = get_logger(__name__)

class ProjectRepository:
    def __init__(self):
        self.db = []
    
    def get_by_id(self, project_id: int) -> Project | None:
        logger.info(f"ProjectRepository fetching project {project_id}")
        return next((p for p in self.db if p.id == project_id), None)
