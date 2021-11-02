import Head from 'next/head'
import Image from 'next/image'
import styles from '../styles/Home.module.css'
import useSWR from 'swr'

const fetcher = (url) => fetch(url).then((res) => res.json())

export default function Home() {
  const { data, error } = useSWR('http://localhost:8080/posts', fetcher)
  if (error) return (<div>error</div>)
  if (!data) return (<div>loading...</div>)
  return (
    <div>{data.response}</div>
  )
}
