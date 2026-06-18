from sqlalchemy.orm import Session
from app.models.inventory import Inventory

class InventoryRepository:
    def __init__(self, db: Session):
        self.db = db

    def get_by_product_id(self, product_id: int):
        return self.db.query(Inventory).filter(Inventory.product_id == product_id).first()

    def update_quantity(self, product_id: int, quantity: int):
        inv = self.get_by_product_id(product_id)
        if not inv:
            inv = Inventory(product_id=product_id, quantity=quantity)
            self.db.add(inv)
        else:
            inv.quantity = quantity
        self.db.commit()
        self.db.refresh(inv)
        return inv
