from fastapi import APIRouter
from controllers.ReportController import ReportController

router = APIRouter()
controller = ReportController()

@router.post("/reports")
def generate_report():
    return controller.generate_report()
