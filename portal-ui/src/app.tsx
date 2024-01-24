import './app.css'

export function App() {
    console.log("config", import.meta.env.VITE_APPLICATION_URL);

    return (
        <>
            <h1>Unicore portal</h1>
            <div><span>Url: </span><span>{import.meta.env.VITE_APPLICATION_URL}</span></div>
        </>
    )
}
