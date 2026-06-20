import logging

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger("taskflow")

def get_logger(name: str):
    return logging.getLogger(name)
