from sqlalchemy.orm import Session
from app.models.order import Order

class OrderRepository:
    def __init__(self, db: Session):
        self.db = db

    def create_order(self, user_id: int, total_amount: float) -> Order:
        order = Order(user_id=user_id, total_amount=total_amount)
        self.db.add(order)
        self.db.commit()
        self.db.refresh(order)
        return order
        
    def get_user_orders(self, user_id: int):
        return self.db.query(Order).filter(Order.user_id == user_id).all()
