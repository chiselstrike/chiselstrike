import axios from "axios"

const chiselStrikeApi = axios.create({
  baseURL: "http://localhost:8080/dev",
})

export default chiselStrikeApi
