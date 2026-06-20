from fastapi import FastAPI
from api.users import router as users_router
from api.projects import router as projects_router
from api.tasks import router as tasks_router
from api.reports import router as reports_router
from utils.logger import get_logger
from services.ProjectService import ProjectService
from services.NotificationService import NotificationService

logger = get_logger(__name__)

app = FastAPI(title="Python Taskflow")

app.include_router(users_router, prefix="/api/v1")
app.include_router(projects_router, prefix="/api/v1")
app.include_router(tasks_router, prefix="/api/v1")
app.include_router(reports_router, prefix="/api/v1")

@app.get("/")
def read_root():
    logger.info("Root endpoint called")
    return {"message": "Welcome to Python Taskflow"}
