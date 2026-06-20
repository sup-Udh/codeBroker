// Everything depends on this
class Logger {
  info(msg) {
    console.log(`[INFO] ${msg}`);
  }
  error(msg) {
    console.error(`[ERROR] ${msg}`);
  }
  warn(msg) {
    console.warn(`[WARN] ${msg}`);
  }
}
module.exports = new Logger();
