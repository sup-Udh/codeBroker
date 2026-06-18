from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy.orm import Session
from app.config.database import get_db
from app.services.order_service import OrderService
from app.schemas.order_schema import OrderCreate, OrderResponse

router = APIRouter(prefix="/orders", tags=["Orders"])

@router.post("/", response_model=OrderResponse)
def place_order(order: OrderCreate, db: Session = Depends(get_db)):
    order_service = OrderService(db)
    try:
        return order_service.place_order(user_id=1, user_email="test@test.com", order_data=order)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
