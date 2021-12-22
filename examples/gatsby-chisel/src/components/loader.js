import React from "react"

function Loader() {
  return (
    <div className="w-full h-full left-0 top-0 absolute bg-slate-700 grid place-items-center opacity-20 border-r-4">
      <div className="loader"></div>
    </div>
  )
}

export default Loader
