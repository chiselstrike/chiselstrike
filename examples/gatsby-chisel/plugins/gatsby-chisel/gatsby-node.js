const { spawn } = require("child_process")

const consoleColorOff = "\x1b[0m"
const consoleColorWhite = "\x1b[37m"
const consoleColorRed = "\x1b[31m"
const consoleColorCyan = "\x1b[36m"

function color(color, msg) {
  return `${color}${msg}${consoleColorOff} `
}

function logNormalData(data) {
  String(data)
    .split("\n")
    .forEach(s =>
      console.log(
        color(consoleColorCyan, "ChiselStrike:"),
        color(consoleColorWhite, s)
      )
    )
}

exports.pluginOptionsSchema = ({ Joi }) => {
  return Joi.object({
    path: Joi.string().required(),
  })
}

exports.onCreateDevServer = (_, options) => {
  const chiselServer = spawn("npm", ["run", "dev"], {
    cwd: options.path,
  })

  chiselServer.stdout.on("data", data => {
    logNormalData(data)
  })

  chiselServer.stderr.on("data", data => {
    logNormalData(data)
  })

  chiselServer.on("error", error => {
    console.error(
      color(consoleColorRed, `ChiselStrike error: ${error.message}`)
    )
  })

  chiselServer.on("close", code => {
    console.log(
      color(
        consoleColorRed,
        `ChiselStrike's server process exited with code ${code}. Killing Gatsby's process soon...`
      )
    )
    // Kills Gatsby along with ChiselStrike, remove this if ChiselStrike needs to fail silently
    process.exit(code ?? 1)
  })
}
