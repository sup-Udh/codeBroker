from fastapi import APIRouter, Depends
from sqlalchemy.orm import Session
from app.config.database import get_db
from app.services.inventory_service import InventoryService
from app.schemas.inventory_schema import InventoryUpdate, InventoryResponse

router = APIRouter(prefix="/inventory", tags=["Inventory"])

@router.put("/{product_id}", response_model=InventoryResponse)
def update_inventory(product_id: int, inv: InventoryUpdate, db: Session = Depends(get_db)):
    inv_service = InventoryService(db)
    # Simulate an admin call
    inv_service.deduct_stock(product_id, -inv.quantity, user_id=1)
    return {"id": 1, "product_id": product_id, "quantity": inv.quantity}
