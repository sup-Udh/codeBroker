from pydantic import BaseModel

class InventoryUpdate(BaseModel):
    quantity: int

class InventoryResponse(BaseModel):
    id: int
    product_id: int
    quantity: int

    class Config:
        from_attributes = True
